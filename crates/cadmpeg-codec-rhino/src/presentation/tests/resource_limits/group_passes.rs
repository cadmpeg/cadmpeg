// SPDX-License-Identifier: Apache-2.0
//! Each group identity pass admits only the next source visit.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::presentation::{disambiguate_group_ids, GroupIdentity, GroupRecord};

const GROUP_COUNT: usize = 2;

fn groups(duplicates: bool) -> Vec<GroupRecord> {
    (0..GROUP_COUNT).map(|order| GroupRecord {
        id: if duplicates { String::new() } else { format!("already-assigned-{order}") },
        identity: GroupIdentity::ArchiveIndex(if duplicates { 7 } else { i32::try_from(order).unwrap() }),
        source_offset: u64::try_from(order).unwrap(),
        archive_index: i32::try_from(order).unwrap(),
        source_uuid: None,
        name: String::new(),
        links: Vec::new(),
    }).collect()
}

fn counts_work() -> u64 {
    // Initial hash growth admits four buckets, alignment padding and control bytes.
    // Two keys fit that first allocation; no live keys move or rehash.
    let first_growth = 4 * std::mem::size_of::<(GroupIdentity, usize)>()
        + std::mem::align_of::<(GroupIdentity, usize)>().max(16) - 1 + 4 + 16;
    // Each visit then admits contains_key and entry: two fixed-width key hashes.
    u64::try_from(first_growth + GROUP_COUNT * (1 + 2 * std::mem::size_of::<GroupIdentity>())).unwrap()
}

fn lookups_work() -> u64 {
    // Each visit is followed by one fixed-width key hash for get.
    u64::try_from(GROUP_COUNT * (1 + std::mem::size_of::<GroupIdentity>())).unwrap()
}

fn assert_visit_refusal(limit: u64, operation: &'static str, mut values: Vec<GroupRecord>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(refusal) = disambiguate_group_ids(&ctx, &mut values, None).unwrap_err()
        else { panic!("group traversal refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, operation);
    assert_eq!((refusal.used, refusal.additional), (limit, 1));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn group_count_pass_refuses_only_the_first_source_visit() {
    assert_visit_refusal(0, "Rhino disambiguate group ids traversal", groups(false));
}

#[test]
fn group_duplicate_lookup_pass_refuses_only_the_first_source_visit() {
    assert_visit_refusal(counts_work(), "Rhino disambiguate group ids traversal", groups(false));
}

#[test]
fn group_duplicate_assignment_pass_refuses_only_the_first_source_visit() {
    assert_visit_refusal(counts_work() + lookups_work(), "Rhino duplicate group traversal", groups(true));
}

#[test]
fn group_final_identity_pass_refuses_only_the_first_source_visit() {
    assert_visit_refusal(counts_work() + lookups_work(), "Rhino group identity assignment", groups(false));
}
