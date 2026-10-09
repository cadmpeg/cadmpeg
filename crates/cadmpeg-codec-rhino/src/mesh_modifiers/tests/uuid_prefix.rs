// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::mesh_modifiers::parse_uuid;
use crate::wire::Uuid;

#[test]
fn modifier_uuid_accepts_actual_bytes_without_exhaustion_work() {
    for text in ["11111111111111111111111111111111", "11111111-1111-1111-1111-111111111111"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The forward and reverse brace searches each stop on their first byte.
        policy.limits.max_work_units = 2 + text.len() as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        assert_eq!(parse_uuid(&ctx, text).unwrap(), Some(Uuid::from_canonical([0x11; 16])));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn modifier_uuid_invalid_first_byte_leaves_the_tail_unvisited() {
    let text = format!("g{}", "1".repeat(8192));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two actual brace-search positions, then the invalid first digit.
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert_eq!(parse_uuid(&ctx, &text).unwrap(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn modifier_uuid_digit_refusal_keeps_the_original_fuse() {
    let text = format!("g{}", "1".repeat(8192));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = parse_uuid(&ctx, &text).unwrap_err() else {
        panic!("actual digit visit must refuse");
    };
    assert_eq!(limit.operation, "Rhino XML UUID digits");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!((limit.used, limit.additional), (2, 1));
    assert!(matches!(parse_uuid(&ctx, ""), Err(CodecError::ResourceLimit(original)) if original == limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn modifier_uuid_last_actual_digit_refuses_one_below() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two brace-search positions plus 31 of the 32 actual digit visits.
    policy.limits.max_work_units = 2 + 31;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = parse_uuid(&ctx, "11111111111111111111111111111111").unwrap_err() else {
        panic!("last actual digit must refuse");
    };
    assert_eq!(limit.operation, "Rhino XML UUID digits");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!((limit.used, limit.additional), (2 + 31, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}
