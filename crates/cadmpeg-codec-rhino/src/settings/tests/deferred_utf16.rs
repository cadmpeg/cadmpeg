// SPDX-License-Identifier: Apache-2.0
//! Deferred UTF-16 validates actual characters without output storage.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::{BoundedReader, FramingError};

#[test]
fn deferred_utf16_empty_and_supplementary_have_no_terminal_work() {
    for (text, characters) in [("", 0), ("é😀x", 3)] {
        let bytes = super::utf16_bytes(text);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = characters;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let value = crate::settings::utf16_deferred(&ctx, &mut reader)
            .expect("one visit per decoded character, no exhaustion visit");
        assert_eq!(value.bytes.len(), text.encode_utf16().count() * 2);
        assert_eq!(reader.remaining(), 0);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn deferred_utf16_refuses_only_the_next_actual_character() {
    let bytes = super::utf16_bytes("é😀x");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let result = crate::settings::utf16_deferred(&ctx, &mut reader);
    let Err(FramingError::Resource(refusal)) = result else { panic!("actual next-character refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "validate Rhino deferred UTF-16");
    assert_eq!((refusal.used, refusal.additional), (2, 1));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn deferred_utf16_invalid_surrogate_keeps_original_location() {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    bytes.extend(0xd83d_u16.to_le_bytes());
    bytes.extend(0_u16.to_le_bytes());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let result = crate::settings::utf16_deferred(&ctx, &mut reader);
    assert!(matches!(result, Err(FramingError::Structural { offset, message })
        if offset == bytes.len() && message == "invalid UTF-16 surrogate sequence"));
    ctx.finish_session().unwrap();
}

#[test]
fn deferred_utf16_empty_source_keeps_original_sticky_refusal() {
    let bytes = super::utf16_bytes("");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original deferred refusal").unwrap_err()
        else { panic!("original work refusal"); };
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert!(matches!(crate::settings::utf16_deferred(&ctx, &mut reader),
        Err(FramingError::Resource(sticky)) if sticky == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
