// SPDX-License-Identifier: Apache-2.0

use super::{find_frame, find_reference_frame, unique_match, UniqueMatch};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::Cell;

const ORIGINAL: &str = "original F3D shared frame callback refusal";

#[test]
fn unique_match_keeps_original_refusal_in_both_searches_before_callback_outcomes() {
    for refused_at in [1, 2] {
        for outcome in 0..3 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                let calls = Cell::new(0);
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
                let result = unique_match(ctx, &[42, 43, 44], |_| {
                    calls.set(calls.get() + 1);
                    if calls.get() != refused_at { return Ok(true); }
                    assert!(matches!(ctx.charge_work(1, ORIGINAL), Err(CodecError::ResourceLimit(_))));
                    match outcome { 0 => Ok(false), 1 => Ok(true), _ => Err(CodecError::Malformed("ordinary frame callback error".into())) }
                }, "test shared unique callbacks");
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("original unique refusal"); };
                assert_eq!(original.operation, ORIGINAL);
                assert_eq!(original.additional, 1);
                assert_eq!(calls.get(), refused_at);
                assert_eq!(ctx.resource_refusal(), Some(original.clone()));
                assert!(matches!(ctx.charge_work(0, "later unique boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
            });
        }
    }
}

fn headers() -> Vec<u8> {
    let mut bytes = Vec::new();
    for class in [b"273", b"258", b"281"] {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class);
        bytes.extend_from_slice(&7_u32.to_le_bytes());
    }
    bytes
}

#[test]
fn frame_and_reference_frame_keep_original_callback_refusal_and_native_byte_order() {
    let bytes = headers();
    let native = bytes.clone();
    for with_reference in [false, true] {
        for outcome in 0..3 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let records = IndexedRecordOffsets::build(&ctx, &bytes).unwrap();
            assert_eq!(records.offsets(7), [0, 11, 22]);
            let calls = Cell::new(0);
            let mut visit = |start, paired| {
                calls.set(calls.get() + 1);
                assert_eq!((start, paired), (0, 11));
                assert!(matches!(ctx.charge_work(1, ORIGINAL), Err(CodecError::ResourceLimit(_))));
                match outcome { 0 => Ok(false), 1 => Ok(true), _ => Err(CodecError::Malformed("ordinary frame callback error".into())) }
            };
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
            let result = if with_reference {
                find_reference_frame(&ctx, &records, &ReferenceRun::unlocated(vec![7, 8]), |record, start, paired| { assert_eq!(record, 7); visit(start, paired) }, "test reference frame callbacks").map(|_| ())
            } else { find_frame(&ctx, &records, 7, &mut visit, "test frame callbacks").map(|_| ()) };
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("original frame refusal"); };
            assert_eq!(original.operation, ORIGINAL);
            assert_eq!(original.additional, 1);
            assert_eq!(calls.get(), 1);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert!(matches!(ctx.charge_work(0, "later frame boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
            assert_eq!(records.offsets(7), [0, 11, 22]);
        }
    }
    assert_eq!(bytes, native);
}

#[test]
fn shared_frame_known_empty_lookup_is_free_and_keeps_preexisting_original_fuse() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let records = IndexedRecordOffsets::build(ctx, &[]).unwrap();
        assert_eq!(find_frame(ctx, &records, 7, |_, _| panic!("no frame callback"), "test empty frame").unwrap(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, ORIGINAL) else { panic!("zero-work refusal"); };
        assert!(matches!(find_frame(ctx, &records, 7, |_, _| panic!("fused frame callback"), "test empty frame"), Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(find_reference_frame(ctx, &records, &ReferenceRun::unlocated(vec![7]), |_, _, _| panic!("fused reference callback"), "test fused reference frame"), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn shared_unique_duplicate_stops_before_suffix_and_preserves_ordinary_error() {
    crate::test_support::with_decode_context(|ctx| {
        let mut visited = Vec::new();
        assert!(matches!(unique_match(ctx, &[42, 43, 44], |value| { visited.push(*value); Ok(true) }, "test shared duplicate order").unwrap(), UniqueMatch::Many));
        assert_eq!(visited, [42, 43]);
        assert!(matches!(unique_match(ctx, &[42], |_| Err(CodecError::Malformed("ordinary frame callback error".into())), "test shared ordinary error"), Err(CodecError::Malformed(message)) if message == "ordinary frame callback error"));
        assert!(ctx.resource_refusal().is_none());
    });
}
