// SPDX-License-Identifier: Apache-2.0
//! Group passes preserve source fields and sticky refusals under scoped storage.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::presentation::{disambiguate_group_ids, GroupIdentity, GroupRecord};

fn groups(duplicates: bool) -> Vec<GroupRecord> {
    (0..2).map(|order| GroupRecord {
        id: String::new(),
        identity: GroupIdentity::ArchiveIndex(if duplicates { 7 } else { order }),
        source_offset: u64::try_from(order).unwrap(),
        archive_index: order,
        source_uuid: None,
        name: String::new(),
        links: Vec::new(),
    }).collect()
}
fn assert_refusal(operation: &'static str, duplicates: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut staging = ctx.reserve_scoped(0, "group test staging").unwrap();
    let mut values = groups(duplicates);
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    let CodecError::ResourceLimit(refusal) = disambiguate_group_ids(&ctx, &mut values, &mut staging).unwrap_err()
        else { panic!("group traversal refusal"); };
    drop(probe);
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, operation);
    if operation != "Rhino group identity lookup" { assert_eq!(refusal.additional, 1); }
    assert!(values.iter().all(|value| value.id.is_empty()));
    assert_eq!(values[0].identity, GroupIdentity::ArchiveIndex(if duplicates { 7 } else { 0 }));
    assert_eq!(values[1].source_offset, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    drop(values);
    drop(staging);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}
#[test]
fn group_count_pass_refuses_only_the_first_source_visit() {
    assert_refusal("Rhino disambiguate group ids traversal", false);
}
#[test]
fn group_duplicate_lookup_pass_refuses_before_assigning_ids() {
    assert_refusal("Rhino group identity lookup", false);
}
#[test]
fn group_duplicate_assignment_pass_refuses_only_the_first_source_visit() {
    assert_refusal("Rhino duplicate group traversal", true);
}
#[test]
fn group_final_identity_pass_refuses_only_the_first_source_visit() {
    assert_refusal("Rhino group identity assignment", false);
}
#[test]
fn group_ids_use_only_scoped_backing_and_release_it() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut staging = ctx.reserve_scoped(0, "group test staging").unwrap();
    let mut values = groups(false);
    assert_eq!(disambiguate_group_ids(&ctx, &mut values, &mut staging).unwrap(), 0);
    assert_eq!(values[0].id, "rhino:presentation:group#index-0");
    assert_eq!(values[1].id, "rhino:presentation:group#index-1");
    drop(values);
    drop(staging);
    let released = ctx.reserve_scoped(4096, "group backing released").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}
