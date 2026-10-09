// SPDX-License-Identifier: Apache-2.0

use super::super::{frame, frame_history, payload_subtype_range, payload_token, scan_history_boundary, Record};
use crate::kernel_header::RefWidth;
use crate::stream_error::StreamFailure;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn manual_sab_history_scan_charges_only_actual_tokens() {
    for count in [1_usize, 64] {
        let mut bytes = vec![0x0d, 1, b'x'];
        bytes.extend(vec![0x0b; count]);
        // One record, one name token, one UTF-8 byte, and n value tokens.
        let required = u64::try_from(3 + count).unwrap();
        for cap in [3, required - 1, required] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_collection_items = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = scan_history_boundary(&ctx, &bytes, 0, RefWidth::Four, None);
            if cap == required {
                assert!(result.unwrap().is_none());
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(first)) = result else { panic!("actual history token refusal"); };
                assert_eq!(first.operation, "scan SAB history token");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                for input in [b"".as_slice(), bytes.as_slice()] {
                    assert!(matches!(scan_history_boundary(&ctx, input, 0, RefWidth::Four, None),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

#[test]
fn manual_sab_framing_keeps_strict_and_history_eof_semantics() {
    for count in [1_usize, 64] {
        let mut bytes = vec![0x0d, u8::try_from(count).unwrap()];
        bytes.extend(vec![b'x'; count]);
        let name = "x".repeat(count);
        let n = u64::try_from(count).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Record and name-token visits, then n UTF-8 bytes. EOF is fixed
        // grammar rejection, with no further token visit.
        policy.limits.max_work_units = 2 + n;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(frame(&ctx, &bytes, 0, bytes.len(), RefWidth::Four, None),
            Err(StreamFailure::Parse(error)) if error.offset == bytes.len() && error.reason == "end of stream"));
        ctx.finish_session().unwrap();
        let arena = DecodeArena::new();
        // The retained name join adds two one-part traversals and n copied
        // bytes. Initial vector reserves move no live slots.
        policy.limits.max_work_units = 4 + 2 * n;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let records = frame_history(&ctx, &bytes, 0, bytes.len(), RefWidth::Four, None).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!((records[0].index, records[0].offset, records[0].len), (0, 0, bytes.len()));
        assert_eq!(records[0].name, name);
        assert!(records[0].tokens.is_empty());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn manual_sab_empty_and_invalid_routes_preserve_original_refusal() {
    let empty = Record { index: 0, name: String::new(), tokens: Vec::new().into(), offset: 0, len: 0 };
    let invalid = Record { offset: usize::MAX, len: 1, ..empty.clone() };
    for fused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if fused {
            let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original SAB scan refusal")
            else { panic!("original refusal"); };
            for _ in 0..64 {
                for record in [&empty, &invalid] {
                    assert!(matches!(payload_token(&ctx, &[], record, RefWidth::Four, 0),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                    assert!(matches!(payload_subtype_range(&ctx, &[], record, 0, RefWidth::Four, "x"),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                }
                for start in [0, usize::MAX] {
                    assert!(matches!(scan_history_boundary(&ctx, &[], start, RefWidth::Four, None),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                    assert!(matches!(frame(&ctx, &[], start, 0, RefWidth::Four, None),
                        Err(StreamFailure::Resource(last)) if last == first));
                    assert!(matches!(frame_history(&ctx, &[], start, 0, RefWidth::Four, None),
                        Err(StreamFailure::Resource(last)) if last == first));
                }
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            for record in [&empty, &invalid] {
                assert!(payload_token(&ctx, &[], record, RefWidth::Four, 0).unwrap().is_none());
                assert!(payload_subtype_range(&ctx, &[], record, 0, RefWidth::Four, "x").unwrap().is_none());
            }
            assert!(scan_history_boundary(&ctx, &[], 0, RefWidth::Four, None).unwrap().is_none());
            assert!(frame(&ctx, &[], 0, 0, RefWidth::Four, None).unwrap().is_empty());
            assert!(frame_history(&ctx, &[], 0, 0, RefWidth::Four, None).unwrap().is_empty());
            ctx.finish_session().unwrap();
        }
    }
}
