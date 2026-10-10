// SPDX-License-Identifier: Apache-2.0

use super::{contains_face, contains_slot};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FaceId;

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn refusal(error: &CodecError, operation: &'static str, limit: u64) -> ResourceLimit {
    let CodecError::ResourceLimit(actual) = error else {
        panic!("expected work refusal: {error:?}");
    };
    assert_eq!(
        *actual,
        ResourceLimit {
            dimension: ResourceDimension::WorkUnits,
            reason: ResourceFailure::BudgetExceeded,
            limit,
            used: limit,
            additional: 1,
            operation,
        }
    );
    *actual
}

#[test]
fn face_membership_first_match_does_not_visit_native_suffix() {
    let target = FaceId::mint("f3d:brep:entity#1").unwrap();
    let unread = FaceId::mint(format!("f3d:brep:entity#{}", "u".repeat(4096))).unwrap();
    let values = [target.clone(), unread];
    let before = serde_json::to_value(&values).unwrap();
    // One concrete slice step, then one admitted comparison per equal byte.
    let work = 1 + u64::try_from(target.as_str().len()).unwrap();
    for allowed in [true, false] {
        let arena = DecodeArena::new();
        let cap = work - u64::from(!allowed);
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = contains_face(&ctx, &values, &target, "first face match");
        if allowed {
            assert!(result.unwrap());
            assert_eq!(ctx.resource_refusal(), None);
            ctx.finish_session().unwrap();
        } else {
            let original = refusal(&result.unwrap_err(), "first face match", cap);
            
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert_eq!(serde_json::to_value(&values).unwrap(), before);
    }
}

#[test]
fn face_membership_first_byte_mismatch_skips_long_equal_length_tails() {
    let tail = "u".repeat(4096);
    let candidate = FaceId::mint(format!("f3d:brep:entity#{tail}")).unwrap();
    let target = FaceId::mint(format!("g3d:brep:entity#{tail}")).unwrap();
    assert_eq!(candidate.as_str().len(), target.as_str().len());
    let values = [candidate];
    let before = serde_json::to_value(&values).unwrap();
    // One slice step, the first differing byte pair, and the slice end probe.
    for cap in [3, 2] {
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = contains_face(&ctx, &values, &target, "first byte mismatch");
        if cap == 3 {
            assert!(!result.unwrap());
            assert_eq!(ctx.resource_refusal(), None);
            ctx.finish_session().unwrap();
        } else {
            let original = refusal(&result.unwrap_err(), "first byte mismatch", cap);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert_eq!(serde_json::to_value(&values).unwrap(), before);
    }
}

#[test]
fn face_membership_unequal_lengths_need_only_source_steps() {
    let candidate = FaceId::mint("f3d:brep:entity#1").unwrap();
    let target = FaceId::mint("f3d:brep:entity#100").unwrap();
    let values = [candidate];
    // Unequal lengths have no byte comparisons: the slot and end probe cost two.
    for cap in [2, 1] {
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = contains_face(&ctx, &values, &target, "unequal face lengths");
        if cap == 2 {
            assert!(!result.unwrap());
            ctx.finish_session().unwrap();
        } else {
            let original = refusal(&result.unwrap_err(), "unequal face lengths", cap);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn face_slot_membership_pays_only_reached_slots_and_terminal_probe() {
    let values = [1, 2, 3];
    // Fixed scalar equality adds no input-sized work to each reached source slot.
    for (target, cap, expected) in [(1, 1, true), (2, 2, true), (4, 4, false)] {
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            contains_slot(&ctx, &values, target, "stable slot query").unwrap(),
            expected
        );
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().unwrap();
    }
    let arena = DecodeArena::new();
    let policy = policy(3);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = refusal(&
        contains_slot(&ctx, &values, 4, "stable slot query").unwrap_err(),
        "stable slot query",
        3,
    );
    assert!(
        matches!(contains_slot(&ctx, &values, 1, "later first slot"),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert_eq!(values, [1, 2, 3]);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}
