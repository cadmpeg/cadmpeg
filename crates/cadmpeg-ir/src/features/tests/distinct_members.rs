// SPDX-License-Identifier: Apache-2.0
use crate::features::DistinctMembers;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn insert_for_decode_service_profile_and_duplicate_without_charge() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut members = DistinctMembers::default();
    assert!(members
        .insert(&ctx, 1_u8, "test member insert")
        .expect("service profile"));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(!members
        .insert(&ctx, 1, "test member duplicate")
        .expect("duplicate needs no slot"));
    assert_eq!(members.as_slice(), &[1]);
}

#[test]
fn member_iterator_reconstruction_uses_the_shared_insertion_algorithm() {
    let mut members: DistinctMembers<_> = [2_u8, 1, 2].into_iter().collect();
    assert_eq!(members.as_slice(), [2, 1]);
    members.extend([1, 3, 2]);
    assert_eq!(members.as_slice(), [2, 1, 3]);
}
