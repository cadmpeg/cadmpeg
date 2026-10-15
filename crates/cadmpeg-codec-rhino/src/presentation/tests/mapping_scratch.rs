// SPDX-License-Identifier: Apache-2.0
//! Primitive wrapper scratch does not become mapping output storage.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::chunks::{ArchiveVersion, FramingError};
use crate::objects::UserdataDescriptor;
use crate::presentation::{parse_texture_mapping, MAPPING_CRC_CACHE};
use crate::test_support::test_dump::{
    class_userdata_v1_with_direct_payload, class_wrapper_with_userdata, MESH_CLASS,
};
use crate::wire::Uuid;

fn mapping_with_cache(cache_version: i32) -> Vec<u8> {
    let mut cache = cache_version.to_le_bytes().to_vec();
    cache.extend(17_i32.to_le_bytes());
    let userdata = class_userdata_v1_with_direct_payload(
        ArchiveVersion::V8, MAPPING_CRC_CACHE.to_wire(), &cache,
    );
    let primitive = class_wrapper_with_userdata(ArchiveVersion::V8, MESH_CLASS, &[], &userdata);
    let mut payload = MESH_CLASS.to_vec();
    payload.extend(6_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    for _ in 0..2 {
        for index in 0..16 {
            payload.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend(primitive);
    super::anonymous(0, &payload)
}

fn userdata_capacity_bytes() -> u64 {
    // The first admitted Vec slot uses the core four-slot growth bound for
    // elements of 2..=1024 bytes. No diagnostic or child-range Vec is needed.
    let item_bytes = std::mem::size_of::<UserdataDescriptor>();
    assert!((2..=1024).contains(&item_bytes));
    u64::try_from(4 * item_bytes).unwrap()
}

fn assert_mapping_scratch_released(cache_version: i32, cache_requires_opaque: bool) {
    let bytes = mapping_with_cache(cache_version);
    let uuid = Uuid::from_wire(MESH_CLASS).to_string();
    let expected_id = format!("rhino:presentation:texture_mapping#{uuid}");
    // Each record has an empty name, one generated ID and two UUID strings.
    let output_bytes = expected_id.len() + 2 * uuid.len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(2 * output_bytes).unwrap();
    policy.limits.max_materialized_bytes = userdata_capacity_bytes();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let first = parse_texture_mapping(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8, 42).unwrap();
    let second = parse_texture_mapping(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8, 43).unwrap();
    for mapping in [&first, &second] {
        assert_eq!(mapping.value.id, expected_id);
        assert_eq!(mapping.value.source_uuid.as_deref(), Some(uuid.as_str()));
        assert_eq!(mapping.value.primitive_class_uuid.as_deref(), Some(uuid.as_str()));
        assert_eq!(mapping.value.name, "");
        assert_eq!(mapping.value.mapping_type, 6);
        assert_eq!(mapping.cache_requires_opaque, cache_requires_opaque);
    }
    let released = ctx.reserve_scoped(userdata_capacity_bytes(), "mapping primitive scratch reuse")
        .expect("both records survive while all primitive parse backing is released");
    drop(released);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn valid_mapping_cache_scratch_is_released_with_both_outputs_live() {
    assert_mapping_scratch_released(1, false);
}

#[test]
fn unsupported_mapping_cache_scratch_is_released_without_losing_opaque_recovery() {
    assert_mapping_scratch_released(2, true);
}

fn assert_userdata_refusal(dimension: ResourceDimension, additional: u64) {
    let bytes = mapping_with_cache(1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
        _ => panic!("fixture covers userdata slots or backing"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let FramingError::Resource(original) = parse_texture_mapping(
        &ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8, 42,
    ).err().expect("userdata is refused") else { panic!("resource refusal"); };
    assert_eq!(original.operation, "Rhino class userdata");
    assert_eq!(original.dimension, dimension);
    assert_eq!(original.used, 0);
    assert_eq!(original.additional, additional);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn mapping_primitive_userdata_refuses_actual_temporary_backing_and_stays_fused() {
    assert_userdata_refusal(ResourceDimension::MaterializedBytes, userdata_capacity_bytes());
}

#[test]
fn mapping_primitive_userdata_keeps_original_one_slot_refusal() {
    assert_userdata_refusal(ResourceDimension::CollectionItems, 1);
}
