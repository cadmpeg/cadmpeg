// SPDX-License-Identifier: Apache-2.0

use super::super::{frame, frame_history, scan_history_boundary};
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
                let Err(CodecError::ResourceLimit(first)) = result else {
                    panic!("actual history token refusal");
                };
                assert_eq!(first.operation, "scan SAB history token");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                assert!(
                    matches!(scan_history_boundary(&ctx, bytes.as_slice(), 0, RefWidth::Four, None),
                        Err(CodecError::ResourceLimit(last)) if last == first)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
                );
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
        assert!(
            matches!(frame(&ctx, &bytes, 0, bytes.len(), RefWidth::Four, None),
            Err(StreamFailure::Parse(error)) if error.offset == bytes.len() && error.reason == "end of stream")
        );
        ctx.finish_session().unwrap();
        let arena = DecodeArena::new();
        // The retained name join adds two one-part traversals and n copied
        // bytes. Name indexing adds n byte visits and one end probe. Initial
        // vector reserves move no live slots.
        policy.limits.max_work_units = 5 + 3 * n;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let records = frame_history(&ctx, &bytes, 0, bytes.len(), RefWidth::Four, None).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            (records[0].index, records[0].offset, records[0].len),
            (0, 0, bytes.len())
        );
        assert_eq!(records[0].name, name);
        assert!(records[0].tokens.is_empty());
        drop(records);
        ctx.finish_session().unwrap();
    }
}
