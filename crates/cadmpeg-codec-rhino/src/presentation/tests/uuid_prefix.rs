// SPDX-License-Identifier: Apache-2.0
//! UUID text admits only bytes that the parser visits.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::presentation::parse_uuid_text;
use crate::wire::Uuid;

#[test]
fn uuid_text_accepts_exact_byte_budget_without_exhaustion_work() {
    for text in ["11111111-1111-1111-1111-111111111111", "--11111111111111111111111111111111---", "11111111111111111111111111111111"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(text.len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(parse_uuid_text(&ctx, text).unwrap(), Some(Uuid::from_canonical([0x11; 16])));
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn uuid_text_invalid_first_byte_does_not_visit_the_tail() {
    let text = format!("x{}", "1".repeat(1024));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(parse_uuid_text(&ctx, &text).unwrap(), None);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn empty_uuid_text_observes_the_original_sticky_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "UUID test first refusal").unwrap_err() else { panic!("work refusal"); };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert!(matches!(parse_uuid_text(&ctx, ""), Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}
