// SPDX-License-Identifier: Apache-2.0

use super::{face, start_geometry_fixture};
use crate::design::face_resolve::retain_face_operand_resolution;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;

#[test]
fn retained_face_membership_refuses_before_native_search_or_mutation() {
    let (mut operand, mut group, _) = start_geometry_fixture();
    let target = face(10);
    let before = serde_json::to_value(&operand).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let expected = ResourceLimit {
        dimension: ResourceDimension::WorkUnits,
        reason: ResourceFailure::BudgetExceeded,
        limit: 0,
        used: 0,
        additional: 1,
        operation: "resolve F3D retained face group stream",
    };
    assert!(matches!(retain_face_operand_resolution(
        &ctx, &group, std::slice::from_mut(&mut operand), &target),
        Err(CodecError::ResourceLimit(actual)) if actual == expected));
    assert_eq!(serde_json::to_value(&operand).unwrap(), before);
    // A missing stream and empty input must not replace the original refusal.
    group.id = "unqualified".into();
    assert!(matches!(retain_face_operand_resolution(&ctx, &group, &mut [], &target),
        Err(CodecError::ResourceLimit(actual)) if actual == expected));
    assert_eq!(ctx.resource_refusal(), Some(expected));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == expected));
}

#[test]
fn retained_face_ambiguous_members_stop_before_unread_native_suffix() {
    let (mut operand, group, _) = start_geometry_fixture();
    let target = face(10);
    operand.candidate_faces = vec![target.clone()];
    let duplicate = operand.clone();
    let mut unread = operand.clone();
    unread.id = format!("f3d:test:face-operand#{}", "u".repeat(4096));
    let mut operands = [operand, duplicate, unread];
    let before = serde_json::to_value(&operands).unwrap();
    // Reverse stream search visits the tail and separator. Each reached
    // operand then visits its slot, stream bytes, first group member, first
    // candidate slot, and the equal face bytes. The second match ends search.
    let group_search = "construction-group#150".len() + 1;
    let per_operand = 1 + "face-operand#200".len() + 1 + "f3d:test".len()
        + 1 + 1 + target.as_str().len();
    let work = u64::try_from(group_search + 2 * per_operand).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!retain_face_operand_resolution(&ctx, &group, &mut operands, &target).unwrap());
    assert_eq!(serde_json::to_value(&operands).unwrap(), before);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn retained_face_second_candidate_comparison_refuses_without_partial_publication() {
    let (mut operand, group, _) = start_geometry_fixture();
    let target = face(10);
    operand.candidate_faces = vec![target.clone()];
    let duplicate = operand.clone();
    let mut operands = [operand, duplicate];
    let before = serde_json::to_value(&operands).unwrap();
    let group_search = "construction-group#150".len() + 1;
    let per_operand = 1 + "face-operand#200".len() + 1 + "f3d:test".len()
        + 1 + 1 + target.as_str().len();
    let work = u64::try_from(group_search + 2 * per_operand).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work - 1;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let expected = ResourceLimit {
        dimension: ResourceDimension::WorkUnits,
        reason: ResourceFailure::BudgetExceeded,
        limit: work - 1,
        used: work - 1,
        additional: 1,
        operation: "find F3D retained face operand candidate",
    };
    assert!(matches!(retain_face_operand_resolution(&ctx, &group, &mut operands, &target),
        Err(CodecError::ResourceLimit(actual)) if actual == expected));
    assert_eq!(serde_json::to_value(&operands).unwrap(), before);
    assert!(matches!(retain_face_operand_resolution(&ctx, &group, &mut [], &target),
        Err(CodecError::ResourceLimit(actual)) if actual == expected));
    assert_eq!(ctx.resource_refusal(), Some(expected));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == expected));
}
