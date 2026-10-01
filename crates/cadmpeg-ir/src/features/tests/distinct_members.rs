// SPDX-License-Identifier: Apache-2.0
use crate::features::DistinctMembers;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn insert_for_decode_refuses_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::default();
    assert!(
        matches!(members.insert_for_decode(&ctx, 1_u8, "test member insert"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems)
    );
    assert_eq!(members.0.capacity(), 0);
    assert!(members.is_empty());
}

#[test]
fn insert_for_decode_service_profile_and_duplicate_without_charge() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::default();
    assert!(members
        .insert_for_decode(&ctx, 1_u8, "test member insert")
        .expect("service profile"));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(!members
        .insert_for_decode(&ctx, 1, "test member duplicate")
        .expect("duplicate needs no slot"));
    assert_eq!(members.as_slice(), &[1]);
}

#[test]
fn reserve_for_decode_refuses_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::<u8>::default();
    assert!(
        matches!(members.reserve_for_decode(&ctx, 2, "test member reserve"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert_eq!(members.0.capacity(), 0);
    assert!(members.is_empty());
}

#[test]
fn reserve_for_decode_service_profile() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::<u8>::default();
    members
        .reserve_for_decode(&ctx, 2, "test member reserve")
        .expect("service profile");
    assert!(members.0.capacity() >= 2);
    assert!(members.is_empty());
}
